//! Spec 006, 3.4: `plan check`, `compile` and `show` on both reference
//! plans, one refusal per category 1 to 12, load diagnostics, and golden
//! stdout. SYNTHETIC inputs only (R-04).

mod common;

use common::*;
use rustev_contract::Document;
use rustev_contract::definition::{
    BindingChoice, CalibrationRef, Cond, Definition, ExactDecl, Fallback, RequiredKind, StepBody,
    StepDecl, UncalibratedThreshold,
};
use rustev_contract::execution::{CostPolicy, Delay, FailureClass};
use rustev_contract::plan::Plan;
use rustev_core::builder::e;
use rustev_core::ops::{Arith, CountWhere, OpArgs};

fn semantic_mut<'a>(
    def: &'a mut Definition,
    id: &str,
) -> &'a mut rustev_contract::definition::SemanticDecl {
    match &mut def.steps.iter_mut().find(|s| s.id == id).unwrap().body {
        StepBody::Semantic(d) => d,
        StepBody::Exact(_) => panic!("{id} is exact"),
    }
}

fn exact_step(id: &str, args: OpArgs) -> StepDecl {
    let (op, version) = args.op();
    StepDecl {
        id: id.into(),
        body: StepBody::Exact(ExactDecl {
            op: op.into(),
            version,
            args: args.to_json(),
        }),
    }
}

const SUPPORT_DEPS: [&str; 4] = [
    "--rules",
    "support.rules.json",
    "--calibration",
    "topic.calibration.json",
];

fn with<'a>(head: &[&'a str], tail: &[&'a str]) -> Vec<&'a str> {
    head.iter().chain(tail).copied().collect()
}

#[test]
fn both_reference_plans_check_compile_and_show_with_golden_output() {
    let s = Scratch::new("plan-golden");
    reference_inputs(&s);
    for (task, deps) in [
        ("support", &SUPPORT_DEPS[..]),
        ("lodging", &["--rules", "lodging.rules.json"][..]),
    ] {
        let def = format!("{task}.definition.json");
        let r = s.rustev(&with(&["plan", "check", "--definition", &def], deps));
        let j = r.expect(0, "ok");
        cli_golden(&format!("{task}.check.json"), &r.stdout);

        let out = format!("{task}.plan.json");
        let r = s.rustev(&with(
            &["plan", "compile", "--definition", &def, "--out", &out],
            deps,
        ));
        let c = r.expect(0, "compiled");
        cli_golden(&format!("{task}.compile.json"), &r.stdout);
        assert_eq!(c["plan_id"], j["plan_id"]);
        // The written plan is the canonical document with that identity.
        let plan = Plan::parse(&s.read(&out)).unwrap();
        assert_eq!(plan.canonical().unwrap(), s.read(&out));
        use rustev_contract::Identified;
        assert_eq!(plan.id().unwrap().to_string(), j["plan_id"]);

        let r = s.rustev(&with(&["plan", "show", "--plan", &out], deps));
        let shown = r.expect(0, "ok");
        cli_golden(&format!("{task}.show.json"), &r.stdout);
        assert_eq!(shown["plan_id"], j["plan_id"]);

        // Compile never overwrites.
        let again = s.rustev(&with(
            &["plan", "compile", "--definition", &def, "--out", &out],
            deps,
        ));
        again.expect(3, "io_error");
    }
}

/// The refusal fixtures: one definition (or execution policy) per category.
fn refusal_inputs(s: &Scratch) -> Vec<(u8, Vec<String>)> {
    let (base, cal) = support_definition();
    let mut cases: Vec<(u8, Definition)> = vec![];
    // 2: an unknown operator version.
    let mut d = base.clone();
    if let StepBody::Exact(x) = &mut d.steps[1].body {
        x.version = 2;
    }
    cases.push((2, d));
    // 3: count_where over a text input.
    let mut d = base.clone();
    d.steps[1] = exact_step(
        "failed_payments_30d",
        OpArgs::CountWhere(CountWhere {
            list: "input:ticket.message".into(),
            predicate: e::all(vec![]),
        }),
    );
    cases.push((3, d));
    // 4: a cycle.
    let mut d = base.clone();
    d.steps.push(exact_step(
        "a",
        OpArgs::Arith(Arith {
            expr: e::sub(e::r("step:b"), e::int(1)),
        }),
    ));
    d.steps.push(exact_step(
        "b",
        OpArgs::Arith(Arith {
            expr: e::sub(e::r("step:a"), e::int(1)),
        }),
    ));
    cases.push((4, d));
    // 5: a named backend no program provides.
    let mut d = base.clone();
    semantic_mut(&mut d, "frustration").backend.binding = BindingChoice::Backend("nope".into());
    cases.push((5, d));
    // 6: a mass threshold without a declared uncalibrated reason.
    let mut d = base.clone();
    if let Cond::Any(cs) = &mut d.policy.adjustments[0].when {
        if let Cond::MassAtLeast {
            uncalibrated_threshold,
            ..
        } = &mut cs[1]
        {
            *uncalibrated_threshold = UncalibratedThreshold::NotDeclared;
        }
    }
    cases.push((6, d));
    // 7: three requests against a ceiling of two.
    let d = support_routing_builder(&cal)
        .limits(limits(2))
        .build()
        .unwrap();
    cases.push((7, d));
    // 8: the frustration handler removed.
    let mut d = base.clone();
    d.policy
        .on_unresolved
        .retain(|h| h.target != "step:frustration");
    cases.push((8, d));
    // 9: a fallback kind a mass threshold cannot accept.
    let mut d = base.clone();
    semantic_mut(&mut d, "explicit_deadline").fallback = Fallback::Kind {
        requires: RequiredKind::Label,
        reason: "labels only".into(),
    };
    cases.push((9, d));
    // 10: a calibrated probability with no calibration named.
    let mut d = base.clone();
    semantic_mut(&mut d, "topic").calibration = CalibrationRef::None;
    cases.push((10, d));
    // 11: a duplicate step id.
    let mut d = base.clone();
    d.steps[1].id = d.steps[0].id.clone();
    cases.push((11, d));

    let deps: Vec<String> = SUPPORT_DEPS.iter().map(|x| x.to_string()).collect();
    let mut out = vec![];
    // 1: a duplicate key in the definition bytes.
    let text = String::from_utf8(canonical(&base)).unwrap();
    let dup = text.replacen("{\"inputs\":", "{\"name\":\"x\",\"inputs\":", 1);
    s.write("cat-01.definition.json", dup.as_bytes());
    out.push((
        1,
        [
            &["--definition".to_string(), "cat-01.definition.json".into()][..],
            &deps,
        ]
        .concat(),
    ));
    for (c, d) in cases {
        let name = format!("cat-{c:02}.definition.json");
        s.write(&name, &canonical(&d));
        out.push((c, [&["--definition".to_string(), name][..], &deps].concat()));
    }
    // 12: an execution policy naming a step the plan does not have.
    s.write("support.definition.json", &canonical(&base));
    let policy = execution(
        CostPolicy::Unlimited,
        vec![step_exec(
            "no_such_step",
            2,
            &[FailureClass::Transient],
            Delay::Fixed { ms: 0 },
            &[],
            &[],
        )],
    );
    s.write("cat-12.execution.json", &canonical(&policy));
    out.push((
        12,
        [
            &[
                "--definition".to_string(),
                "support.definition.json".into(),
                "--execution".into(),
                "cat-12.execution.json".into(),
            ][..],
            &deps,
        ]
        .concat(),
    ));
    out
}

#[test]
fn one_refusal_per_category_with_golden_output() {
    let s = Scratch::new("plan-refusals");
    reference_inputs(&s);
    let cases = refusal_inputs(&s);
    assert_eq!(
        cases.iter().map(|(c, _)| *c).collect::<Vec<_>>(),
        (1..=12).collect::<Vec<u8>>()
    );
    for (category, args) in cases {
        let argv: Vec<&str> = ["plan", "check"]
            .into_iter()
            .chain(args.iter().map(String::as_str))
            .collect();
        let r = s.rustev(&argv);
        let j = r.expect(5, "refused");
        assert_eq!(j["category"], category, "{}", r.stdout_str());
        cli_golden(&format!("refusal-{category:02}.json"), &r.stdout);
        // Compile refuses the same way and writes nothing.
        let mut argv = argv.clone();
        argv[1] = "compile";
        argv.extend(["--out", "refused.plan.json"]);
        let r = s.rustev(&argv);
        assert_eq!(r.json()["category"], category);
        assert!(!s.exists("refused.plan.json"));
    }
}

#[test]
fn malformed_dependencies_are_category_1_naming_the_file() {
    let s = Scratch::new("plan-deps");
    reference_inputs(&s);
    s.write("bad.descriptor.json", b"{\"schema\":\"rustev.backend/1\"");
    s.write("bad.calibration.json", b"[]");
    s.write("bad.execution.json", b"{}");
    s.write("bad.rules.json", b"{\"schema\":\"rustev.rules/1\"}");
    for (flag, file, subject) in [
        (
            "--descriptor",
            "bad.descriptor.json",
            "descriptor:bad.descriptor.json",
        ),
        (
            "--calibration",
            "bad.calibration.json",
            "calibration:bad.calibration.json",
        ),
        (
            "--execution",
            "bad.execution.json",
            "execution:bad.execution.json",
        ),
    ] {
        let r = s.rustev(&with(
            &[
                "plan",
                "check",
                "--definition",
                "support.definition.json",
                flag,
                file,
            ],
            &SUPPORT_DEPS,
        ));
        let j = r.expect(5, "refused");
        assert_eq!(
            (j["category"].as_u64(), j["subject"].as_str()),
            (Some(1), Some(subject))
        );
    }
    let r = s.rustev(&[
        "plan",
        "check",
        "--definition",
        "support.definition.json",
        "--rules",
        "bad.rules.json",
    ]);
    let j = r.expect(4, "invalid_input");
    assert_eq!(j["input"], "bad.rules.json");
}

#[test]
fn show_reports_load_diagnostics_and_never_prints_an_unreproducible_plan() {
    let s = Scratch::new("plan-show");
    reference_inputs(&s);
    s.rustev(&with(
        &[
            "plan",
            "compile",
            "--definition",
            "support.definition.json",
            "--out",
            "p.json",
        ],
        &SUPPORT_DEPS,
    ))
    .expect(0, "compiled");
    let plan = Plan::parse(&s.read("p.json")).unwrap();

    // Without the calibration.
    let r = s.rustev(&[
        "plan",
        "show",
        "--plan",
        "p.json",
        "--rules",
        "support.rules.json",
    ]);
    assert_eq!(r.expect(5, "refused")["load_error"], "missing_calibration");
    // Without the program.
    let r = s.rustev(&[
        "plan",
        "show",
        "--plan",
        "p.json",
        "--calibration",
        "topic.calibration.json",
    ]);
    assert_eq!(r.expect(5, "refused")["load_error"], "missing_descriptor");
    // An edited embedded definition.
    let mut edited = plan.clone();
    edited.definition.version = "1.0.1".into();
    s.write("edited.json", &edited.canonical().unwrap());
    let r = s.rustev(&with(
        &["plan", "show", "--plan", "edited.json"],
        &SUPPORT_DEPS,
    ));
    assert_eq!(r.expect(5, "refused")["load_error"], "plan_mismatch");
    // Another compiler.
    let mut other = plan.clone();
    other.compiler = "rustev-core/9.9.9".into();
    s.write("other.json", &other.canonical().unwrap());
    let r = s.rustev(&with(
        &["plan", "show", "--plan", "other.json"],
        &SUPPORT_DEPS,
    ));
    let j = r.expect(5, "refused");
    assert_eq!(j["load_error"], "compiler_changed");
    assert_eq!(j["identities"]["recorded_compiler"], "rustev-core/9.9.9");
    // A malformed plan document is a category 1 refusal.
    s.write("junk.json", b"{");
    let r = s.rustev(&with(
        &["plan", "show", "--plan", "junk.json"],
        &SUPPORT_DEPS,
    ));
    let j = r.expect(5, "refused");
    assert_eq!(
        (j["category"].as_u64(), j["subject"].as_str()),
        (Some(1), Some("plan:junk.json"))
    );
}

#[test]
fn a_definition_one_byte_past_its_limit_is_a_parse_refusal() {
    let s = Scratch::new("plan-limit");
    reference_inputs(&s);
    let limit = rustev_contract::limits::DEFINITION_V1.max_bytes;
    // Valid JSON padded with whitespace to exactly the limit, then one more.
    let mut def = canonical(&support_definition().0);
    def.resize(limit, b' ');
    s.write("at-limit.json", &def);
    let r = s.rustev(&with(
        &["plan", "check", "--definition", "at-limit.json"],
        &SUPPORT_DEPS,
    ));
    r.expect(0, "ok");
    def.push(b' ');
    s.write("past-limit.json", &def);
    let r = s.rustev(&with(
        &["plan", "check", "--definition", "past-limit.json"],
        &SUPPORT_DEPS,
    ));
    let j = r.expect(5, "refused");
    assert_eq!(j["category"], 1);
    assert!(j["detail"].as_str().unwrap().contains("bytes"), "{j}");
}

#[test]
fn unreadable_inputs_are_io_errors_naming_the_path() {
    let s = Scratch::new("plan-io");
    reference_inputs(&s);
    let r = s.rustev(&["plan", "check", "--definition", "absent.json"]);
    assert_eq!(r.expect(3, "io_error")["path"], "absent.json");
    std::fs::create_dir(s.path("a-dir")).unwrap();
    let r = s.rustev(&["plan", "check", "--definition", "a-dir"]);
    assert_eq!(r.expect(3, "io_error")["path"], "a-dir");
    let r = s.rustev(&with(
        &[
            "plan",
            "compile",
            "--definition",
            "support.definition.json",
            "--out",
            "no-dir/p.json",
        ],
        &SUPPORT_DEPS,
    ));
    assert_eq!(r.expect(3, "io_error")["path"], "no-dir/p.json");
}
