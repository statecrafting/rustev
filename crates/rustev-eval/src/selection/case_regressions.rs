//! Per-case selection evidence (spec 064).
use super::*;
use crate::eval_cases::{form_in, metric_cases};
use crate::repeat_stats::{Form, MetricCases, run_stats};

const LIST_LIMIT: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaseResults {
    pub compared: u64,
    pub unmeasured: u64,
    pub regressions: CaseList<Regression>,
    pub unresolved: CaseList<UnresolvedCase>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "CaseListOf{T}")]
pub struct CaseList<T> {
    pub total: u64,
    pub truncated: bool,
    pub list: Vec<T>,
}
impl<T> CaseList<T> {
    fn new() -> Self {
        Self {
            total: 0,
            truncated: false,
            list: vec![],
        }
    }
    fn push(&mut self, entry: T) {
        self.total += 1;
        if self.list.len() < LIST_LIMIT {
            self.list.push(entry);
        } else {
            self.truncated = true;
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Regression {
    pub case: String,
    pub baseline: f64,
    pub candidate: f64,
    pub drop: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnresolvedCase {
    pub case: String,
    pub reason: String,
    pub side: String,
}
// Preserve the difference between an absent member and explicit null on read.
pub(super) fn read_cases<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<Option<CaseResults>>, D::Error> {
    Option::<CaseResults>::deserialize(d).map(Some)
}

/// The stdout view of either kind of candidate record. Existing clauses retain
/// their record bytes; only per-case lists become bounded summaries (064 3.5.3).
pub fn stdout_candidates<T: Serialize>(gate: &SelectionGate, candidates: &T) -> serde_json::Value {
    let mut records = serde_json::to_value(candidates).expect("selection records serialize");
    for record in records.as_array_mut().expect("candidate list") {
        for entry in record["clauses"].as_array_mut().expect("clause list") {
            let label = entry["clause"].as_str().expect("clause label");
            let limit = [
                ("require", gate.require.as_slice()),
                ("and_any", gate.and_any.as_deref().unwrap_or(&[])),
            ]
            .into_iter()
            .find_map(|(group, clauses)| {
                clauses.iter().enumerate().find_map(|(i, c)| {
                    if label != format!("{group}[{i}]") {
                        return None;
                    }
                    match c {
                        Clause::Metric(MetricClause::CaseRegressions {
                            case_regressions, ..
                        }) => Some(case_regressions.at_most),
                        _ => None,
                    }
                })
            });
            if let Some(at_most) = limit {
                let cases = &entry["cases"];
                if !cases.is_null() {
                    let first: Vec<_> = cases["regressions"]["list"]
                        .as_array()
                        .expect("regressions")
                        .iter()
                        .take(10)
                        .map(|r| serde_json::json!({"case":r["case"], "drop":r["drop"]}))
                        .collect();
                    entry["cases"] = serde_json::json!({"regressions":cases["regressions"]["total"], "at_most":at_most, "unresolved":cases["unresolved"]["total"], "first":first});
                }
            }
        }
    }
    records
}

#[derive(Debug, PartialEq)]
enum CaseValue {
    Unmeasured,
    Partial,
    Unknown,
    Known(f64),
}
fn case_value(m: &MetricCases, index: usize) -> CaseValue {
    let runs = &m.cases[index];
    let present = runs.iter().filter(|r| r.is_some()).count();
    if present == 0 {
        return CaseValue::Unmeasured;
    }
    if present != runs.len() {
        return CaseValue::Partial;
    }
    let one = MetricCases {
        name: m.name.clone(),
        form: m.form.clone(),
        cases: vec![runs.clone()],
    };
    match run_stats(&one, runs.len(), false)
        .ok()
        .and_then(|s| s.pooled.value())
    {
        Some(v) if v.is_finite() => CaseValue::Known(v),
        _ => CaseValue::Unknown,
    }
}

/// Positive exact difference, written as a plain decimal without trailing zeros.
fn drop_string(high: f64, low: f64) -> String {
    let (h, l) = (
        shortest(high).expect("finite"),
        shortest(low).expect("finite"),
    );
    let exponent = h.exponent.min(l.exponent).min(0);
    let (mut pos, mut neg) = (vec![], vec![]);
    for (term, subtract) in [(h, false), (l, true)] {
        let mut digits = vec![0; (term.exponent - exponent) as usize];
        digits.extend(term.digits.iter().rev());
        add_digits(
            if term.negative != subtract {
                &mut neg
            } else {
                &mut pos
            },
            &digits,
        );
    }
    let mut borrow = 0i16;
    for (i, digit) in pos.iter_mut().enumerate() {
        let n = i16::from(*digit) - i16::from(neg.get(i).copied().unwrap_or(0)) - borrow;
        *digit = n.rem_euclid(10) as u8;
        borrow = i16::from(n < 0);
    }
    while pos.last() == Some(&0) {
        pos.pop();
    }
    let fraction = (-exponent) as usize;
    pos.resize(pos.len().max(fraction + 1), 0);
    let mut s: String = pos.iter().rev().map(|d| char::from(b'0' + d)).collect();
    if fraction > 0 {
        s.insert(s.len() - fraction, '.');
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    s
}

pub(super) fn evaluate(
    label: String,
    name: &str,
    limit: &CaseRegressionLimit,
    direction: Direction,
    b: &Side<'_>,
    c: &Side<'_>,
) -> ClauseResult {
    let mut entry = result(label, unknown("not-case-comparable"));
    entry.cases = Some(None);
    let Some(baseline) = b.cases.first() else {
        entry.result = unknown("baseline has no case documents");
        return entry;
    };
    let Some(candidate) = c.cases.first() else {
        entry.result = unknown("candidate has no case documents");
        return entry;
    };
    if !baseline
        .cases
        .iter()
        .map(|c| &c.case)
        .eq(candidate.cases.iter().map(|c| &c.case))
    {
        entry.result = unknown("baseline and candidate case id lists differ in count or order");
        return entry;
    }
    let docs: Vec<_> = b.cases.iter().chain(&c.cases).copied().collect();
    let form = crate::host_run::form(name)
        .filter(|_| b.host)
        .or_else(|| form_in(name, &docs))
        // These declared metric families have count or mean measures. When
        // every measure is absent or unknown, their form cannot affect a value.
        .or_else(|| {
            ["rubric/", "model/rubric/", "constraint/"]
                .iter()
                .any(|p| name.starts_with(p))
                .then_some(Form::Mean)
        });
    let Some(form @ (Form::CountRatio | Form::Mean)) = form else {
        entry.result = unknown(format!("not-case-comparable: {name}"));
        return entry;
    };
    let observation = if c.host {
        crate::host_repeat::observed(name)
    } else {
        observed(name)
    };
    if observation
        && !metric(c, "all", name).is_ok_and(|m| m.observation == Some(Observation::Candidate))
    {
        entry.result = unknown(format!(
            "{name}: historical or missing candidate observation"
        ));
        return entry;
    }
    let (bm, cm) = match (
        metric_cases(name, form.clone(), &b.cases),
        metric_cases(name, form, &c.cases),
    ) {
        (Ok(bm), Ok(cm)) => (bm, cm),
        _ => {
            entry.result = unknown("case lists differ");
            return entry;
        }
    };
    let mut cases = CaseResults {
        compared: 0,
        unmeasured: 0,
        regressions: CaseList::new(),
        unresolved: CaseList::new(),
    };
    for (i, case) in baseline.cases.iter().enumerate() {
        let (bv, cv) = (case_value(&bm, i), case_value(&cm, i));
        match (&bv, &cv) {
            (CaseValue::Unmeasured, CaseValue::Unmeasured) => cases.unmeasured += 1,
            (CaseValue::Known(bv), CaseValue::Known(cv)) => {
                cases.compared += 1;
                let (high, low) = if direction == Direction::HigherIsBetter {
                    (*bv, *cv)
                } else {
                    (*cv, *bv)
                };
                if exact_sum_cmp(
                    &[shortest(high).expect("finite")],
                    &[shortest(low).expect("finite"), exact(limit.by.absolute)],
                )
                .is_ge()
                {
                    cases.regressions.push(Regression {
                        case: case.case.clone(),
                        baseline: *bv,
                        candidate: *cv,
                        drop: drop_string(high, low),
                    });
                }
            }
            _ => {
                let (side, value) = match (&bv, &cv) {
                    (CaseValue::Partial | CaseValue::Unknown, _) => ("baseline", &bv),
                    (_, CaseValue::Partial | CaseValue::Unknown) => ("candidate", &cv),
                    (CaseValue::Unmeasured, _) => ("baseline", &bv),
                    _ => ("candidate", &cv),
                };
                let reason = match value {
                    CaseValue::Partial => "partial",
                    CaseValue::Unknown => "unknown",
                    _ => "unpaired",
                };
                cases.unresolved.push(UnresolvedCase {
                    case: case.case.clone(),
                    reason: reason.into(),
                    side: side.into(),
                });
            }
        }
    }
    entry.result = if cases.regressions.total > limit.at_most {
        fail("case regressions exceed at_most")
    } else if let Some(first) = cases.unresolved.list.first() {
        unknown(format!("{}: {} ({})", first.case, first.reason, first.side))
    } else {
        pass("case regressions within at_most")
    };
    entry.cases = Some(Some(cases));
    entry
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval_cases::{CaseMeasure, CaseMeasures, EVAL_CASES, EvalCases};
    use crate::repeat_stats::Contribution;
    fn mean(n: f64) -> Option<CaseMeasure> {
        Some(CaseMeasure::Mean { num: n, den: 1 })
    }
    fn docs(name: &str, cases: &[Vec<Option<CaseMeasure>>]) -> Vec<EvalCases> {
        (0..cases[0].len())
            .map(|r| EvalCases {
                schema: EVAL_CASES.into(),
                report: ContentDigest::parse(&format!("sha256:{:064x}", 1)).unwrap(),
                top_k: None,
                cases: cases
                    .iter()
                    .enumerate()
                    .map(|(i, c)| CaseMeasures {
                        case: format!("case-{i}"),
                        run: None,
                        outcome: "proposal:yes".into(),
                        top_k: None,
                        measures: c[r]
                            .clone()
                            .map(|v| BTreeMap::from([(name.into(), v)]))
                            .unwrap_or_default(),
                    })
                    .collect(),
            })
            .collect()
    }
    fn evaluate_docs(
        name: &str,
        b: &[EvalCases],
        c: &[EvalCases],
        host: bool,
        at_most: u64,
    ) -> ClauseResult {
        let side = |cases| Side {
            cohorts: &[],
            split_role: SplitRole::Validation,
            held_out: None,
            opaque: &|_| false,
            host,
            cases,
        };
        evaluate(
            "require[0]".into(),
            name,
            &CaseRegressionLimit {
                by: AbsoluteDrop {
                    absolute: Decimal::parse("0.1").unwrap(),
                },
                at_most,
            },
            Direction::HigherIsBetter,
            &side(b.iter().collect()),
            &side(c.iter().collect()),
        )
    }
    #[test]
    fn case_value_is_bit_identical_to_one_case_pool_for_both_forms_and_run_counts() {
        for n in [1, 3] {
            for (form, contributions) in [
                (
                    Form::Mean,
                    vec![
                        Some(Contribution::Mean { num: 0.1, den: 1 }),
                        Some(Contribution::Mean { num: 0.7, den: 3 }),
                        Some(Contribution::Mean { num: 0.9, den: 7 }),
                    ],
                ),
                (
                    Form::CountRatio,
                    vec![
                        Some(Contribution::Count { num: 1, den: 3 }),
                        Some(Contribution::Count { num: 2, den: 7 }),
                        Some(Contribution::Count { num: 1, den: 9 }),
                    ],
                ),
            ] {
                let m = MetricCases {
                    name: "metric".into(),
                    form,
                    cases: vec![contributions[..n].to_vec()],
                };
                let CaseValue::Known(v) = case_value(&m, 0) else {
                    panic!("known")
                };
                assert_eq!(
                    v.to_bits(),
                    run_stats(&m, n, false)
                        .unwrap()
                        .pooled
                        .value()
                        .unwrap()
                        .to_bits()
                );
            }
        }
    }
    #[test]
    fn unresolved_reasons_and_failure_dominance_detect_mutation_seeds() {
        for host in [false, true] {
            for (candidate, reason) in [
                (vec![None, None, None], "unpaired"),
                (vec![mean(0.5), None, mean(0.5)], "partial"),
                (
                    vec![
                        mean(0.5),
                        Some(CaseMeasure::Unknown {
                            reason: "missing".into(),
                        }),
                        mean(0.5),
                    ],
                    "unknown",
                ),
            ] {
                let b = docs("mean_log_loss", &[vec![mean(0.5); 3], vec![mean(0.5); 3]]);
                let c = docs("mean_log_loss", &[vec![mean(0.25); 3], candidate]);
                let r = evaluate_docs("mean_log_loss", &b, &c, host, 1);
                assert!(
                    matches!(&r.result, ClauseOutcome::Unknown { reason: r } if r.contains("case-1") && r.contains(reason))
                );
                let cs = r.cases.unwrap().unwrap();
                assert_eq!(cs.compared, 1);
                assert_eq!(cs.unresolved.total, 1);
                assert_eq!(cs.unresolved.list[0].reason, reason);
                assert_eq!(cs.unresolved.list[0].side, "candidate");
                let r = evaluate_docs("mean_log_loss", &b, &c, host, 0);
                assert!(matches!(r.result, ClauseOutcome::Fail { .. }));
                // Both sides unresolved names the baseline.
                let r = evaluate_docs("mean_log_loss", &c, &c, host, 1);
                let cs = r.cases.unwrap().unwrap();
                if reason == "unpaired" {
                    assert_eq!(cs.unmeasured, 1);
                    assert_eq!(cs.unresolved.total, 0);
                } else {
                    assert_eq!(cs.unresolved.list[0].side, "baseline");
                }
            }
        }
    }
    #[test]
    fn partial_and_unknown_take_precedence_over_unpaired_on_either_side() {
        let name = "mean_log_loss";
        let absent = docs(name, &[vec![None; 3]]);
        let partial = docs(name, &[vec![mean(0.5), None, mean(0.5)]]);
        let unknown = docs(
            name,
            &[vec![
                Some(CaseMeasure::Unknown {
                    reason: "missing".into()
                });
                3
            ]],
        );
        for host in [false, true] {
            for (measured, reason) in [(&partial, "partial"), (&unknown, "unknown")] {
                for (b, c, side) in [
                    (&absent, measured, "candidate"),
                    (measured, &absent, "baseline"),
                ] {
                    let r = evaluate_docs(name, b, c, host, 0);
                    assert!(
                        matches!(&r.result, ClauseOutcome::Unknown { reason: r } if r.contains(reason) && r.contains(side))
                    );
                    let cases = r.cases.unwrap().unwrap();
                    assert_eq!(cases.unresolved.list[0].reason, reason);
                    assert_eq!(cases.unresolved.list[0].side, side);
                }
            }
            for (b, c, reason) in [
                (&partial, &unknown, "partial"),
                (&unknown, &partial, "unknown"),
            ] {
                let cases = evaluate_docs(name, b, c, host, 0).cases.unwrap().unwrap();
                assert_eq!(cases.unresolved.list[0].reason, reason);
                assert_eq!(cases.unresolved.list[0].side, "baseline");
            }
        }
    }
    #[test]
    fn missing_documents_and_mismatched_case_lists_are_unknown_without_case_evidence() {
        let name = "mean_log_loss";
        let base = docs(name, &[vec![mean(0.5)], vec![mean(0.4)]]);
        let mut shorter = base.clone();
        shorter[0].cases.pop();
        let mut reordered = base.clone();
        reordered[0].cases.reverse();
        let mut renamed = base.clone();
        renamed[0].cases[0].case = "different".into();
        for host in [false, true] {
            for (b, c, reason) in [
                (&[][..], base.as_slice(), "baseline has no case documents"),
                (base.as_slice(), &[][..], "candidate has no case documents"),
                (&[][..], &[][..], "baseline has no case documents"),
                (base.as_slice(), shorter.as_slice(), "case id lists differ"),
                (shorter.as_slice(), base.as_slice(), "case id lists differ"),
                (
                    base.as_slice(),
                    reordered.as_slice(),
                    "case id lists differ",
                ),
                (base.as_slice(), renamed.as_slice(), "case id lists differ"),
            ] {
                let r = evaluate_docs(name, b, c, host, 0);
                assert!(
                    matches!(&r.result, ClauseOutcome::Unknown { reason: r } if r.contains(reason))
                );
                assert_eq!(r.cases, Some(None));
            }
        }
    }
    #[test]
    fn zero_denominator_nonfinite_and_overflow_never_become_values() {
        for contributions in [
            vec![Some(Contribution::Mean { num: 1.0, den: 0 })],
            vec![Some(Contribution::Mean {
                num: f64::INFINITY,
                den: 1,
            })],
            vec![
                Some(Contribution::Mean {
                    num: f64::MAX,
                    den: 1
                });
                3
            ],
        ] {
            let m = MetricCases {
                name: "metric".into(),
                form: Form::Mean,
                cases: vec![contributions],
            };
            assert_eq!(case_value(&m, 0), CaseValue::Unknown);
        }
    }
    #[test]
    fn non_case_kinds_are_unknown_before_cases_are_read() {
        for (name, host) in [
            ("latency_ms/p0.95", false),
            ("host/latency_ms/p0.5", true),
            ("cost_observed_units", false),
            ("agreement/a/qwk", false),
            ("other", true),
        ] {
            let b = docs(name, &[vec![Some(CaseMeasure::Sample { value: 1.0 })]]);
            let r = evaluate_docs(name, &b, &b, host, 0);
            assert!(
                matches!(r.result,ClauseOutcome::Unknown { reason } if reason == format!("not-case-comparable: {name}"))
            );
            assert_eq!(r.cases, Some(None));
        }
    }
    #[test]
    fn bounded_lists_and_stdout_keep_manifest_order_and_full_totals() {
        for host in [false, true] {
            let b = docs("mean_log_loss", &vec![vec![mean(0.5)]; 10_003]);
            for missing in [false, true] {
                let c = docs(
                    "mean_log_loss",
                    &vec![vec![if missing { None } else { mean(0.4) }]; 10_003],
                );
                let r = evaluate_docs("mean_log_loss", &b, &c, host, 0);
                let cs = r.cases.as_ref().unwrap().as_ref().unwrap();
                if missing {
                    assert_eq!(cs.unresolved.total, 10_003);
                    assert!(cs.unresolved.truncated);
                    assert_eq!(cs.unresolved.list.len(), 10_000);
                    assert_eq!(cs.unresolved.list[9999].case, "case-9999");
                    assert!(matches!(r.result, ClauseOutcome::Unknown { .. }));
                } else {
                    assert_eq!(cs.regressions.total, 10_003);
                    assert!(cs.regressions.truncated);
                    assert_eq!(cs.regressions.list.len(), 10_000);
                    assert_eq!(cs.regressions.list[9999].case, "case-9999");
                    assert!(matches!(r.result, ClauseOutcome::Fail { .. }));
                    let g: SelectionGate = serde_json::from_value(serde_json::json!({"schema":SELECTION_GATE,"gate":"test","per":"variant","metrics":{"mean_log_loss":"higher_is_better"},"min_comparable_coverage":"0","min_labeled_coverage":"0","require":[{"metric":"mean_log_loss","case_regressions":{"by":{"absolute":"0.1"},"at_most":0}}],"rank_by":[]})).unwrap();
                    let projected = stdout_candidates(&g, &serde_json::json!([{"clauses":[r]}]));
                    let summary = &projected[0]["clauses"][0]["cases"];
                    assert_eq!(summary["regressions"], 10_003);
                    assert_eq!(summary["first"].as_array().unwrap().len(), 10);
                    assert_eq!(
                        summary["first"][9],
                        serde_json::json!({"case":"case-9","drop":"0.1"})
                    );
                }
            }
        }
    }
    #[test]
    fn exact_drops_across_signs_scales_and_extremes() {
        for (h, l, want) in [
            (0.3, 0.2, "0.1"),
            (0.2, -0.3, "0.5"),
            (-0.2, -0.3, "0.1"),
            (10.0, 9.999999999, "0.000000001"),
            (1e20, 1.0, "99999999999999999999"),
        ] {
            assert_eq!(drop_string(h, l), want);
        }
        assert_eq!(
            drop_string(1e-323, 5e-324),
            format!("0.{}5", "0".repeat(323))
        );
    }
}
