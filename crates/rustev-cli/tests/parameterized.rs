//! Synthetic spec 034 CLI acceptance, using the committed core templates.
mod common;
use common::*;
use serde_json::{Value, json};
const TEMPLATE: &[u8] =
    include_bytes!("../../rustev-core/tests/golden/parameterized.template.json");
const ON: &[u8] = include_bytes!("../../rustev-core/tests/golden/parameterized.profile-on.json");
const OFF: &[u8] = include_bytes!("../../rustev-core/tests/golden/parameterized.profile-off.json");
fn setup() -> Scratch {
    let s = Scratch::new("parameters");
    s.write("template", TEMPLATE);
    s.write("on", ON);
    s.write("off", OFF);
    s.write("descriptor", &canonical(&descriptors()[0]));
    s
}
fn write(s: &Scratch, path: &str, v: &Value) {
    s.write(path, &serde_json::to_vec(v).unwrap());
}
fn golden_run(name: &str, r: &Ran, code: i32, status: &str) -> Value {
    let v = r.expect(code, status);
    cli_golden(&format!("parameterized.{name}.json"), &r.stdout);
    v
}
fn compile(s: &Scratch, profile: &str, variant: &str, out: &str) -> Ran {
    s.rustev(&[
        "plan",
        "compile",
        "--definition",
        "template",
        "--profile",
        profile,
        "--variant",
        variant,
        "--descriptor",
        "descriptor",
        "--out",
        out,
    ])
}
fn diff(s: &Scratch, name: &str, args: &[&str]) -> Value {
    let mut command = vec!["plan", "diff"];
    command.extend(args);
    command.extend(["--out", "diff"]);
    let r = s.rustev(&command);
    let v = golden_run(name, &r, 0, "diffed");
    let bytes = s.read("diff");
    cli_golden(&format!("parameterized.{name}.document.json"), &bytes);
    let doc: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["identical"], doc["identical"]);
    assert_eq!(v["left_plan_id"], doc["left"]["plan_id"]);
    assert_eq!(
        v["counts"]["parameters"],
        doc["parameters"].as_array().unwrap().len()
    );
    std::fs::remove_file(s.path("diff")).unwrap();
    doc
}
#[test]
fn compile_check_show_and_variant_requirement() {
    let s = setup();
    let r = s.rustev(&[
        "plan",
        "compile",
        "--definition",
        "template",
        "--out",
        "plan",
    ]);
    let v = golden_run("variant-required", &r, 5, "refused");
    assert_eq!(v["code"], "variant-required");
    assert!(!s.exists("plan"));
    golden_run(
        "compile",
        &compile(&s, "on", "interactive", "plan"),
        0,
        "compiled",
    );
    assert_eq!(
        s.read("plan"),
        include_bytes!("../../rustev-core/tests/golden/parameterized.on.interactive.plan.json")
    );
    golden_run(
        "show",
        &s.rustev(&[
            "plan",
            "show",
            "--plan",
            "plan",
            "--descriptor",
            "descriptor",
        ]),
        0,
        "ok",
    );
    golden_run(
        "all-variants",
        &s.rustev(&[
            "plan",
            "check",
            "--definition",
            "template",
            "--descriptor",
            "descriptor",
        ]),
        0,
        "ok",
    );
    golden_run(
        "selected",
        &s.rustev(&[
            "plan",
            "check",
            "--definition",
            "template",
            "--variant",
            "batch",
            "--descriptor",
            "descriptor",
        ]),
        0,
        "ok",
    );
    // No clobber, even if input is absent.
    let original = s.read("plan");
    s.rustev(&[
        "plan",
        "compile",
        "--definition",
        "missing",
        "--out",
        "plan",
    ])
    .expect(3, "io_error");
    assert_eq!(original, s.read("plan"));
}
#[test]
fn profile_reports_and_dangling_variant_entries() {
    let s = setup();
    golden_run(
        "profile-changes",
        &s.rustev(&[
            "profile",
            "check",
            "--definition",
            "template",
            "--profile",
            "off",
        ]),
        0,
        "ok",
    );
    golden_run(
        "profile-variants",
        &s.rustev(&["profile", "check", "--definition", "template"]),
        0,
        "ok",
    );
    let mut p: Value = serde_json::from_slice(OFF).unwrap();
    p["values"] = json!({"unit":1000});
    write(&s, "locked", &p);
    golden_run(
        "profile-locked",
        &s.rustev(&[
            "profile",
            "check",
            "--definition",
            "template",
            "--profile",
            "locked",
        ]),
        5,
        "refused",
    );
    let mut t: Value = serde_json::from_slice(TEMPLATE).unwrap();
    t["variants"] = json!(["a", "b", "c"]);
    t["steps"][0]["only_in"] = json!(["a", "b", "c"]);
    t["policy"]["rules"][0]["only_in"] = json!(["b"]);
    t["policy"]["rules"][0]["when"] = json!({"exact":{"cmp":{"left":{"ref":"step:optional"},"op":"ge","right":{"lit":{"integer":0}}}}});
    write(&s, "template", &t);
    for command in ["profile", "plan"] {
        let mut args = vec![
            command,
            "check",
            "--definition",
            "template",
            "--profile",
            "off",
        ];
        if command == "plan" {
            args.extend(["--descriptor", "descriptor"]);
        }
        let v = golden_run(
            &format!("{command}-dangling"),
            &s.rustev(&args),
            5,
            "refused",
        );
        assert_eq!(v["first_refused"], "b");
        assert_eq!(v["variants"][2]["name"], "c");
        if command == "plan" {
            assert!(v["variants"][0].get("plan_id").is_some());
            let b = &v["variants"][1];
            assert_eq!(b["code"], "dangling-reference");
            assert_eq!(b["stage"], "instantiate");
            assert!(b.get("plan_id").is_none() && b.get("pruned").is_none());
        }
    }
}
#[test]
fn compile_refusal_parameters_and_variant_category_mapping() {
    let s = setup();
    let mut t = serde_json::to_value(lodging()).unwrap();
    t["schema"] = json!("rustev.definition/2");
    t["variants"] = json!(["interactive", "batch"]);
    t["parameters"] = json!([{"name":"threshold","type":"decimal","lock":"bounded","bounds":{"range":["-1","1"]},"default":{"value":"0.5"}}]);
    t["steps"][7]["body"]["exact"]["args"]["components"][0]["weight"] =
        json!({"param":"threshold"});
    for index in [1, 2] {
        t["steps"][7]["body"]["exact"]["args"]["components"][index]["weight"] = json!("0");
    }
    write(&s, "template", &t);
    let mut p: Value = serde_json::from_slice(ON).unwrap();
    p["definition"] = json!({"name":"lodging-recommendation","version":"1.0.0"});
    p["values"] = json!({"threshold":"-1"});
    write(&s, "bad", &p);
    let v = golden_run(
        "compile-refused",
        &compile(&s, "bad", "batch", "plan"),
        5,
        "refused",
    );
    assert_eq!(v["stage"], "compile");
    assert_eq!(v["parameters"], json!(["threshold"]));
    assert!(!s.exists("plan"));
    let all = golden_run(
        "variants-compile-refused",
        &s.rustev(&[
            "plan",
            "check",
            "--definition",
            "template",
            "--profile",
            "bad",
            "--descriptor",
            "descriptor",
        ]),
        5,
        "refused",
    );
    assert_eq!(all["variants"][0]["code"], v["category_name"]);
    assert_eq!(all["variants"][0]["stage"], "compile");
}
#[test]
fn both_diff_modes_and_parameter_variant_changes() {
    let s = setup();
    compile(&s, "on", "interactive", "left").expect(0, "compiled");
    compile(&s, "off", "interactive", "right").expect(0, "compiled");
    for (name, right, profile) in [("identical", "left", "on"), ("changed", "right", "off")] {
        let doc = diff(
            &s,
            &format!("plans-{name}"),
            &["--left-plan", "left", "--right-plan", right],
        );
        let compiled = diff(
            &s,
            &format!("definitions-{name}"),
            &[
                "--definition",
                "template",
                "--left-profile",
                "on",
                "--right-profile",
                profile,
                "--left-variant",
                "interactive",
                "--right-variant",
                "interactive",
                "--descriptor",
                "descriptor",
            ],
        );
        assert_eq!(doc, compiled);
    }
    compile(&s, "on", "batch", "batch").expect(0, "compiled");
    let plans = diff(
        &s,
        "plans-pruned",
        &["--left-plan", "left", "--right-plan", "batch"],
    );
    let defs = diff(
        &s,
        "definitions-pruned",
        &[
            "--definition",
            "template",
            "--left-profile",
            "on",
            "--right-profile",
            "on",
            "--left-variant",
            "interactive",
            "--right-variant",
            "batch",
            "--descriptor",
            "descriptor",
        ],
    );
    assert_eq!(plans, defs);
    assert_eq!(plans["steps"]["removed"], json!(["optional"]));
    // Defaults on an absent side profile.
    diff(
        &s,
        "defaults",
        &[
            "--definition",
            "template",
            "--left-variant",
            "interactive",
            "--right-variant",
            "batch",
            "--descriptor",
            "descriptor",
        ],
    );
}
#[test]
fn diff_binding_and_truncated_leaves() {
    let s = setup();
    compile(&s, "on", "interactive", "left").expect(0, "compiled");
    let mut descriptor = descriptors()[0].clone();
    descriptor.determinism = rustev_contract::definition::Determinism::Bitwise;
    s.write("descriptor", &canonical(&descriptor));
    compile(&s, "on", "interactive", "right").expect(0, "compiled");
    let d = diff(
        &s,
        "binding",
        &["--left-plan", "left", "--right-plan", "right"],
    );
    assert_eq!(d["bindings"].as_array().unwrap().len(), 1);
    let mut l: Value = serde_json::from_slice(&s.read("left")).unwrap();
    let mut r = l.clone();
    l["definition"]["steps"][0]["body"]["exact"]["args"] = json!({"values":vec![0;90]});
    r["definition"]["steps"][0]["body"]["exact"]["args"] = json!({"values":vec![1;90]});
    write(&s, "left", &l);
    write(&s, "right", &r);
    let d = diff(
        &s,
        "truncated",
        &["--left-plan", "left", "--right-plan", "right"],
    );
    assert_eq!(d["steps"]["changed"][0]["truncated"], true);
    assert_eq!(
        d["steps"]["changed"][0]["leaves"].as_array().unwrap().len(),
        64
    );
}
#[test]
fn usage_precedes_reads_and_diff_checks_are_ordered() {
    let s = setup();
    for command in ["plan", "profile"] {
        for flags in [
            vec!["--profile-base", "missing"],
            vec!["--definition-version", "2.1.0"],
            vec!["--definition", "missing2"],
        ] {
            let mut args = vec![command, "check", "--definition", "missing"];
            args.extend(flags);
            let r = s.rustev(&args);
            assert_eq!(r.code, 2, "{r:?}");
            assert!(r.stdout.is_empty());
        }
    }
    for (args, message) in [
        (vec![], "exactly one"),
        (
            vec!["--left-plan", "missing", "--left-profile", "missing"],
            "exactly one",
        ),
        (
            vec!["--left-plan", "missing", "--profile-base", "missing"],
            "exactly one",
        ),
        (vec!["--left-plan", "missing"], "--right-plan"),
        (
            vec!["--definition", "missing", "--out", "diff"],
            "side profile or variant",
        ),
        (vec!["--left-plan", "a", "--right-plan", "b"], "--out"),
    ] {
        let mut cmd = vec!["plan", "diff"];
        cmd.extend(args);
        let r = s.rustev(&cmd);
        assert_eq!(r.code, 2, "{r:?}");
        assert!(r.stderr.contains(message), "{}", r.stderr);
        assert!(!s.exists("diff"));
    }
    for flag in ["--definition", "--profile-base"] {
        let mut args = vec![
            "plan",
            "check",
            "--definition",
            "missing",
            "--profile",
            "missing",
        ];
        for _ in 0..65 {
            args.extend([flag, "missing"]);
        }
        assert_eq!(s.rustev(&args).code, 2);
    }
}
#[test]
fn diff_failures_name_side_and_never_write() {
    let s = setup();
    compile(&s, "on", "batch", "good").expect(0, "compiled");
    s.write("bad", b"{}");
    for (left, right, code, status, side) in [
        ("missing", "good", 3, "io_error", "left"),
        ("good", "missing", 3, "io_error", "right"),
        ("bad", "good", 4, "invalid_input", "left"),
        ("good", "bad", 4, "invalid_input", "right"),
    ] {
        let v = s
            .rustev(&[
                "plan",
                "diff",
                "--left-plan",
                left,
                "--right-plan",
                right,
                "--out",
                "diff",
            ])
            .expect(code, status);
        assert_eq!(v["side"], side);
        assert!(!s.exists("diff"));
    }
    for (left, right, side) in [("bad", "batch", "left"), ("batch", "bad", "right")] {
        let v = s
            .rustev(&[
                "plan",
                "diff",
                "--definition",
                "template",
                "--left-variant",
                left,
                "--right-variant",
                right,
                "--descriptor",
                "descriptor",
                "--out",
                "diff",
            ])
            .expect(5, "refused");
        assert_eq!(v["side"], side);
        assert!(!s.exists("diff"));
    }
    s.write("diff", b"keep");
    s.rustev(&[
        "plan",
        "diff",
        "--left-plan",
        "missing",
        "--right-plan",
        "missing",
        "--out",
        "diff",
    ])
    .expect(3, "io_error");
    assert_eq!(s.read("diff"), b"keep");
}

#[test]
fn definition_mode_binding_versions_and_truncated_rules() {
    let s = setup();
    let mut t: Value = serde_json::from_slice(TEMPLATE).unwrap();
    t["steps"][1]["body"]["semantic"]["backend"]["binding"] =
        json!({"backend":"synthetic-linear-head"});
    write(&s, "template", &t);
    t["version"] = json!("2.2.0");
    t["steps"][1]["body"]["semantic"]["backend"]["binding"] =
        json!({"backend":"synthetic-linear-head-replica"});
    write(&s, "newer", &t);
    s.write("replica", &canonical(&linear_head_replica()));
    let mut p: Value = serde_json::from_slice(ON).unwrap();
    p["definition"]["version"] = json!("2.1.0");
    write(&s, "old-profile", &p);
    p["definition"]["version"] = json!("2.2.0");
    write(&s, "new-profile", &p);
    let d = diff(
        &s,
        "definitions-binding",
        &[
            "--definition",
            "template",
            "--definition",
            "newer",
            "--left-profile",
            "old-profile",
            "--right-profile",
            "new-profile",
            "--left-variant",
            "batch",
            "--right-variant",
            "batch",
            "--descriptor",
            "descriptor",
            "--descriptor",
            "replica",
        ],
    );
    assert_eq!(d["bindings"].as_array().unwrap().len(), 1);
    let mut t: Value = serde_json::from_slice(TEMPLATE).unwrap();
    let condition = t["policy"]["rules"][0]["when"]["all"][0].clone();
    t["policy"]["rules"][0]["when"] = json!({"all":vec![condition;90]});
    write(&s, "template", &t);
    let mut p: Value = serde_json::from_slice(OFF).unwrap();
    p["values"]["cutoff"] = json!(3);
    write(&s, "changed", &p);
    let d = diff(
        &s,
        "definitions-truncated",
        &[
            "--definition",
            "template",
            "--left-profile",
            "on",
            "--right-profile",
            "changed",
            "--left-variant",
            "batch",
            "--right-variant",
            "batch",
            "--descriptor",
            "descriptor",
        ],
    );
    assert_eq!(d["rules"]["changed"][0]["truncated"], true);
    assert_eq!(
        d["rules"]["changed"][0]["leaves"].as_array().unwrap().len(),
        64
    );
}

#[test]
fn chain_pin_execution_and_bounded_profile_reads() {
    let s = setup();
    let mut t: Value = serde_json::from_slice(TEMPLATE).unwrap();
    t["version"] = json!("2.2.0");
    write(&s, "newer", &t);
    let mut p: Value = serde_json::from_slice(ON).unwrap();
    p["profile"] = json!("leaf");
    p["extends"] = json!("off@1.0.0");
    write(&s, "leaf", &p);
    let v = s
        .rustev(&[
            "plan",
            "check",
            "--definition",
            "template",
            "--definition",
            "newer",
            "--profile",
            "leaf",
            "--profile-base",
            "off",
            "--definition-version",
            "2.1.0",
            "--variant",
            "batch",
            "--descriptor",
            "descriptor",
        ])
        .expect(0, "ok");
    assert_eq!(v["definition_version"], "2.1.0");
    assert_eq!(v["chain"]["chain"].as_array().unwrap().len(), 2);
    s.write(
        "oversized",
        &vec![b' '; rustev_contract::limits::DESCRIPTOR_V1.max_bytes + 1],
    );
    let v = s
        .rustev(&[
            "profile",
            "check",
            "--definition",
            "template",
            "--profile",
            "oversized",
        ])
        .expect(5, "refused");
    assert_eq!(v["code"], "invalid-profile");
    s.write("execution", b"{}");
    let v = s
        .rustev(&[
            "plan",
            "compile",
            "--definition",
            "template",
            "--variant",
            "batch",
            "--descriptor",
            "descriptor",
            "--execution",
            "execution",
            "--out",
            "plan",
        ])
        .expect(5, "refused");
    assert_eq!(v["subject"], "execution:execution");
    assert!(!s.exists("plan"));
}
