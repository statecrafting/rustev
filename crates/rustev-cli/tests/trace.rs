//! Spec 028, 3.11 and section 5: `rustev trace tail` through the binary,
//! over the span lines `rustev-otel` commits as its goldens. SYNTHETIC only.

mod common;

use std::path::Path;

use common::*;
use rustev_contract::Document;
use rustev_contract::redaction::RedactionPolicy;
use serde_json::{Value as Json, json};

fn otel_golden(name: &str) -> Vec<u8> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../integrations/rustev-otel/tests/golden")
        .join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn root(j: &Json) -> &Json {
    &j["spans"][0]
}

fn find<'a>(n: &'a Json, name: &str, attr: &str, value: &str) -> Option<&'a Json> {
    if n["name"] == name && n["attributes"][attr]["v"] == value {
        return Some(n);
    }
    n["children"]
        .as_array()?
        .iter()
        .find_map(|c| find(c, name, attr, value))
}

#[test]
fn the_last_complete_root_renders_as_a_tree_under_the_default_policy() {
    let s = Scratch::new("trace-default");
    let mut bytes = otel_golden("lodging.trace.jsonl");
    bytes.extend(otel_golden("support.trace.jsonl"));
    s.write("t.jsonl", &bytes);
    let j = s
        .rustev(&["trace", "tail", "--file", "t.jsonl"])
        .expect(0, "rendered");
    assert_eq!(j["command"], "trace tail");
    assert_eq!(j["policy_mismatch"], false);
    let r = root(&j);
    assert_eq!(r["name"], "rustev.decision");
    assert_eq!(
        r["attributes"]["rustev.outcome.action"]["v"],
        "route_ticket"
    );
    // A digest written as a digest is shown as one.
    assert_eq!(r["attributes"]["rustev.decision.id"]["r"], "digest");
    // The tree: steps and the policy span under the root, attempts under
    // their steps.
    let kids = r["children"].as_array().unwrap();
    assert_eq!(kids.len(), 8);
    assert_eq!(kids[7]["name"], "rustev.policy");
    let topic = find(r, "rustev.step", "rustev.step.id", "topic").unwrap();
    assert_eq!(topic["children"][0]["name"], "rustev.attempt");
    assert_eq!(j["trace_id"].as_str().unwrap().len(), 32);
    // A trace id selects an earlier root.
    let first: Json = serde_json::from_slice(
        otel_golden("lodging.trace.jsonl")
            .split(|b| *b == b'\n')
            .next()
            .unwrap(),
    )
    .unwrap();
    let id = first["trace_id"].as_str().unwrap();
    let j = s
        .rustev(&["trace", "tail", "--file", "t.jsonl", "--trace-id", id])
        .expect(0, "rendered");
    assert_eq!(
        root(&j)["attributes"]["rustev.outcome.action"]["v"],
        "present_ranked_lodging"
    );
}

#[test]
fn the_reader_never_shows_more_than_its_policy() {
    let s = Scratch::new("trace-narrow");
    s.write("t.jsonl", &otel_golden("support.allow.trace.jsonl"));
    let j = s
        .rustev(&["trace", "tail", "--file", "t.jsonl"])
        .expect(0, "rendered");
    // The line was written under another policy.
    assert_eq!(j["policy_mismatch"], true);
    let r = root(&j);
    // Marked `allow` in the line, dropped by the reader's policy: withheld.
    assert!(
        r["attributes"]
            .get("rustev.outcome.param.failed_payments_30d")
            .is_none()
    );
    // Raw where the reader's policy says digest, with no key: withheld.
    assert!(r["attributes"].get("rustev.decision.id").is_none());
    let w: Vec<&str> = r["withheld_by_reader"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(w.contains(&"rustev.decision.id"));
    assert!(w.contains(&"rustev.input.source"));
    // A policy that allows them by name shows them.
    let mut p = RedactionPolicy::default();
    p.classes.insert(
        rustev_contract::redaction::FieldClass::Id,
        rustev_contract::redaction::Action::Truncate { bytes: 4 },
    );
    s.write("p.json", &p.canonical().unwrap());
    let j = s
        .rustev(&[
            "trace",
            "tail",
            "--file",
            "t.jsonl",
            "--redaction",
            "p.json",
        ])
        .expect(0, "rendered");
    assert_eq!(
        root(&j)["attributes"]["rustev.decision.id"],
        json!({"v": "supp", "r": "truncated"})
    );
}

#[test]
fn a_key_outside_the_catalogue_is_withheld_with_a_notice() {
    let s = Scratch::new("trace-unknown");
    let text = String::from_utf8(otel_golden("support.allow.trace.jsonl")).unwrap();
    let edited = text.replacen(
        "{\"attributes\":{",
        "{\"attributes\":{\"user.email\":{\"r\":\"allow\",\"v\":\"CANARY\"},",
        1,
    );
    s.write("t.jsonl", edited.as_bytes());
    let r = s.rustev(&["trace", "tail", "--file", "t.jsonl"]);
    let j = r.expect(0, "rendered");
    assert_eq!(root(&j)["not_in_catalogue"], 1);
    assert!(!r.stdout_str().contains("CANARY"));
    // The same key in the resource is counted on the resource.
    let edited = text.replacen(
        "\"resource\":{\"attributes\":{",
        "\"resource\":{\"attributes\":{\"user.email\":{\"r\":\"allow\",\"v\":\"CANARY\"},",
        1,
    );
    assert_ne!(edited, text);
    s.write("r.jsonl", edited.as_bytes());
    let r = s.rustev(&["trace", "tail", "--file", "r.jsonl"]);
    let j = r.expect(0, "rendered");
    assert_eq!(root(&j)["resource"]["not_in_catalogue"], 1);
    assert!(root(&j).get("not_in_catalogue").is_none(), "{j}");
    assert!(!r.stdout_str().contains("CANARY"));
}

#[test]
fn malformed_lines_and_missing_traces_are_invalid_input() {
    let s = Scratch::new("trace-bad");
    let good = otel_golden("support.trace.jsonl");
    // Line 3 malformed.
    let mut lines: Vec<&[u8]> = good.split(|b| *b == b'\n').collect();
    lines[2] = b"{not json";
    s.write("bad.jsonl", &lines.join(&b'\n'));
    let j = s
        .rustev(&["trace", "tail", "--file", "bad.jsonl"])
        .expect(4, "invalid_input");
    assert_eq!(j["line"], 3);
    // Another schema.
    let text = String::from_utf8(good.clone()).unwrap();
    s.write(
        "schema.jsonl",
        text.replacen("rustev.trace/1", "rustev.trace/2", 1)
            .as_bytes(),
    );
    let j = s
        .rustev(&["trace", "tail", "--file", "schema.jsonl"])
        .expect(4, "invalid_input");
    assert_eq!(j["line"], 1);
    // A line over 64 KiB.
    let mut big = good.clone();
    big.extend(vec![b' '; 65 * 1024]);
    big.extend(b"\n");
    s.write("big.jsonl", &big);
    let j = s
        .rustev(&["trace", "tail", "--file", "big.jsonl"])
        .expect(4, "invalid_input");
    assert_eq!(j["line"], 13);
    // No such trace, and an incomplete decision (no policy span).
    s.write("t.jsonl", &good);
    let j = s
        .rustev(&[
            "trace",
            "tail",
            "--file",
            "t.jsonl",
            "--trace-id",
            "0123456789abcdef0123456789abcdef",
        ])
        .expect(4, "invalid_input");
    assert_eq!(j["reason"], "trace_not_found");
    let first = good.split(|b| *b == b'\n').next().unwrap();
    s.write("partial.jsonl", &[first, b"\n"].concat());
    let j = s
        .rustev(&["trace", "tail", "--file", "partial.jsonl"])
        .expect(4, "invalid_input");
    assert_eq!(j["reason"], "trace_not_found");
    // Missing and non-regular files are I/O errors.
    s.rustev(&["trace", "tail", "--file", "absent.jsonl"])
        .expect(3, "io_error");
    std::fs::create_dir(s.path("dir")).unwrap();
    s.rustev(&["trace", "tail", "--file", "dir"])
        .expect(3, "io_error");
}

#[test]
fn a_cyclic_or_too_deep_span_tree_is_invalid_input_not_a_crash() {
    let s = Scratch::new("trace-cycle");
    let good = String::from_utf8(otel_golden("support.trace.jsonl")).unwrap();
    // The root names one of its own steps (line 6) as its parent: following
    // children from the root would come back to it.
    let cyclic = good.replacen(
        "\"parent_span_id\":null",
        "\"parent_span_id\":\"82103443d70753a7\"",
        1,
    );
    assert_ne!(cyclic, good);
    s.write("cycle.jsonl", cyclic.as_bytes());
    let j = s
        .rustev(&["trace", "tail", "--file", "cycle.jsonl"])
        .expect(4, "invalid_input");
    assert_eq!(j["line"], 1);
    assert!(
        j["detail"].as_str().unwrap().contains("reached twice"),
        "{j}"
    );
    // A chain of 70 attempts under the last attempt (line 11, depth 2):
    // the span at depth 64 is the 62nd appended line, line 74.
    let attempt = good.lines().nth(10).unwrap();
    let mut deep = good.clone();
    let mut parent = "5b11449558fa4f7a".to_string();
    for k in 0..70u32 {
        let id = format!("{:016x}", 0x1000 + k);
        let line = attempt
            .replacen(
                "\"span_id\":\"5b11449558fa4f7a\"",
                &format!("\"span_id\":\"{id}\""),
                1,
            )
            .replacen(
                "\"parent_span_id\":\"93b27a9d1f8df5c4\"",
                &format!("\"parent_span_id\":\"{parent}\""),
                1,
            );
        assert_ne!(line, attempt);
        deep.push_str(&line);
        deep.push('\n');
        parent = id;
    }
    s.write("deep.jsonl", deep.as_bytes());
    let j = s
        .rustev(&["trace", "tail", "--file", "deep.jsonl"])
        .expect(4, "invalid_input");
    assert_eq!(j["line"], 74, "{j}");
    assert!(j["detail"].as_str().unwrap().contains("deeper"), "{j}");
    s.rustev(&["trace", "tail", "--file", "deep.jsonl", "--text"])
        .expect(4, "invalid_input");
}

#[test]
fn follow_waits_for_the_rest_of_a_decision_and_times_out() {
    let s = Scratch::new("trace-follow");
    let good = otel_golden("support.trace.jsonl");
    let first_end = good.iter().position(|b| *b == b'\n').unwrap() + 1;
    s.write("t.jsonl", &good[..first_end]);
    let path = s.path("t.jsonl");
    let rest = good[first_end..].to_vec();
    let writer = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(500));
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        f.write_all(&rest).unwrap();
    });
    let j = s
        .rustev(&[
            "trace",
            "tail",
            "--file",
            "t.jsonl",
            "--follow",
            "--timeout-ms",
            "10000",
        ])
        .expect(0, "rendered");
    writer.join().unwrap();
    assert_eq!(root(&j)["children"].as_array().unwrap().len(), 8);
    // Nothing appended: not found when the timeout passes.
    s.write("p.jsonl", &good[..first_end]);
    let j = s
        .rustev(&[
            "trace",
            "tail",
            "--file",
            "p.jsonl",
            "--follow",
            "--timeout-ms",
            "400",
        ])
        .expect(4, "invalid_input");
    assert_eq!(j["reason"], "trace_not_found");
}

#[test]
fn text_prints_an_indented_tree_one_span_per_line() {
    let s = Scratch::new("trace-text");
    s.write("t.jsonl", &otel_golden("support.trace.jsonl"));
    let r = s.rustev(&["trace", "tail", "--file", "t.jsonl", "--text"]);
    assert_eq!(r.code, 0);
    let out = r.stdout_str();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 12);
    assert!(lines[0].starts_with("rustev.decision [unset]"));
    assert!(lines[1].starts_with("  rustev.step [unset]"));
    assert!(lines.iter().any(|l| l.starts_with("    rustev.attempt")));
    assert!(lines[11].starts_with("  rustev.policy"));
}

#[test]
fn flag_rules_are_usage_errors() {
    let s = Scratch::new("trace-usage");
    for args in [
        vec!["trace", "tail"],
        vec!["trace", "tail", "--file", "t", "--timeout-ms", "10"],
        vec![
            "trace",
            "tail",
            "--file",
            "t",
            "--follow",
            "--timeout-ms",
            "0",
        ],
        vec![
            "trace",
            "tail",
            "--file",
            "t",
            "--follow",
            "--timeout-ms",
            "3600001",
        ],
        vec!["trace", "tail", "--file", "t", "--follow", "--follow"],
        vec!["trace", "tail", "--file", "t", "--text", "x"],
        vec!["trace", "tail", "--file", "t", "--trace-id", "ABC"],
        vec![
            "trace",
            "tail",
            "--file",
            "t",
            "--trace-id",
            "0123456789ABCDEF0123456789abcdef",
        ],
    ] {
        let r = s.rustev(&args);
        assert_eq!(r.code, 2, "{args:?}: {}", r.stdout_str());
        assert!(r.stdout.is_empty());
    }
}

#[test]
fn an_invalid_redaction_policy_is_invalid_input() {
    let s = Scratch::new("trace-policy");
    s.write("t.jsonl", &otel_golden("support.trace.jsonl"));
    s.write(
        "p.json",
        br#"{"schema":"rustev.redaction/1","classes":{"user-supplied":"allow"}}"#,
    );
    let j = s
        .rustev(&[
            "trace",
            "tail",
            "--file",
            "t.jsonl",
            "--redaction",
            "p.json",
        ])
        .expect(4, "invalid_input");
    assert_eq!(j["input"], "p.json");
}
