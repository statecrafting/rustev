//! Spec 006, 3.6 and 3.7: capture into bundles under each retention mode,
//! offline replay under an explicitly supplied trusted scope, the isolation
//! matrix through the flags, and the store's availability and confinement.
//! SYNTHETIC only.

mod common;

use common::*;
use rustev_contract::Document;
use rustev_contract::replay::ReplayBundle;
use rustev_contract::retention::Retention;
use serde_json::Value as Json;

const WALL_ARG: &str = "1800000000000";
/// An hour after creation: every default lifetime is still live.
const LATER: &str = "1800003600000";

fn principal(tenant: &str, rev: &str, p: &str) -> Vec<String> {
    [
        "--tenant",
        tenant,
        "--context-revision",
        rev,
        "--principal-scope",
        p,
    ]
    .iter()
    .map(|x| x.to_string())
    .collect()
}

fn tenant_only(tenant: &str, rev: &str) -> Vec<String> {
    [
        "--scope",
        "tenant-only",
        "--tenant",
        tenant,
        "--context-revision",
        rev,
    ]
    .iter()
    .map(|x| x.to_string())
    .collect()
}

/// Run `task` with capture on under `scope` and `retain`, writing
/// `<name>.bundle.json`.
fn capture(s: &Scratch, task: &str, name: &str, scope: &[String], retain: &[&str]) -> Json {
    let mut a = run_args(task, name, &format!("{name}.record.json"));
    a.extend(
        [
            "--capture-bytes",
            "16777216",
            "--bundle-out",
            &format!("{name}.bundle.json"),
            "--now-ms",
            WALL_ARG,
        ]
        .iter()
        .map(|x| x.to_string()),
    );
    a.extend(scope.iter().cloned());
    a.extend(retain.iter().map(|x| x.to_string()));
    s.rustev_owned(&a).expect(0, "judged")
}

fn replay(s: &Scratch, name: &str, scope: &[String], extra: &[&str]) -> Ran {
    let mut a: Vec<String> = [
        "replay",
        "--bundle",
        &format!("{name}.bundle.json"),
        "--now-ms",
    ]
    .iter()
    .map(|x| x.to_string())
    .collect();
    a.push(
        extra
            .first()
            .filter(|x| x.starts_with("18"))
            .map_or(LATER, |v| v)
            .to_string(),
    );
    a.extend(scope.iter().cloned());
    a.extend(
        extra
            .iter()
            .filter(|x| !x.starts_with("18"))
            .map(|x| x.to_string()),
    );
    s.rustev_owned(&a)
}

#[test]
fn every_retention_mode_writes_a_bundle_and_only_retained_bytes_reproduce() {
    let s = Scratch::new("replay-modes");
    run_inputs(&s);
    std::fs::create_dir(s.path("store")).unwrap();
    let scope = principal("tenant-a", "rev-1", "scope-p");
    for task in ["support", "lodging"] {
        let emb = format!("{task}-emb");
        let run = capture(&s, task, &emb, &scope, &["--retain", "embedded"]);
        assert_eq!(run["capture"]["status"], "complete");
        assert_eq!(run["bundle"], format!("{emb}.bundle.json"));
        let r = replay(&s, &emb, &scope, &[]);
        let j = r.expect(0, "reproduced");
        // The reproduced judgment is byte-identical to the run's.
        assert_eq!(j["judgment"], run["judgment"], "{task}");
        assert_eq!(j["supplies"], run["capture"]["entries"]);
        cli_golden(&format!("{task}.replay.json"), &r.stdout);

        let ext = format!("{task}-ext");
        capture(
            &s,
            task,
            &ext,
            &scope,
            &["--retain", "external", "--store", "store"],
        );
        let r = replay(&s, &ext, &scope, &["--store", "store"]);
        assert_eq!(r.expect(0, "reproduced")["judgment"], run["judgment"]);
        // Without the store, external items are inaccessible.
        let r = replay(&s, &ext, &scope, &[]);
        assert_eq!(r.expect(10, "incomparable")["reason"], "inaccessible");

        let dig = format!("{task}-dig");
        capture(&s, task, &dig, &scope, &[]);
        let r = replay(&s, &dig, &scope, &[]);
        assert_eq!(r.expect(10, "incomparable")["reason"], "missing");
    }
}

fn external_refs(s: &Scratch, name: &str) -> Vec<String> {
    let b = ReplayBundle::parse(&s.read(&format!("{name}.bundle.json"))).unwrap();
    b.items()
        .into_iter()
        .filter_map(|(_, item)| match &item.retention {
            Retention::External { reference, .. } => Some(reference.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn store_items_removed_altered_linked_or_expired_are_incomparable() {
    let s = Scratch::new("replay-store");
    run_inputs(&s);
    let scope = principal("tenant-a", "rev-1", "scope-p");
    for (i, (case, expect)) in [
        ("removed", "missing"),
        ("altered", "corrupt"),
        ("linked", "inaccessible"),
    ]
    .into_iter()
    .enumerate()
    {
        let store = format!("store-{i}");
        std::fs::create_dir(s.path(&store)).unwrap();
        let name = format!("st-{case}");
        capture(
            &s,
            "support",
            &name,
            &scope,
            &["--retain", "external", "--store", &store],
        );
        let refs = external_refs(&s, &name);
        assert!(!refs.is_empty());
        // The plan document is resolved first; tamper with its item.
        let target = s.path(&format!("{store}/{}", refs[0]));
        match case {
            "removed" => std::fs::remove_file(&target).unwrap(),
            "altered" => {
                let mut b = std::fs::read(&target).unwrap();
                b.push(b' ');
                std::fs::write(&target, b).unwrap();
            }
            _ => {
                let copy = s.path("elsewhere.json");
                std::fs::rename(&target, &copy).unwrap();
                #[cfg(unix)]
                std::os::unix::fs::symlink(&copy, &target).unwrap();
                #[cfg(not(unix))]
                return;
            }
        }
        let r = replay(&s, &name, &scope, &["--store", &store]);
        assert_eq!(r.expect(10, "incomparable")["reason"], expect, "{case}");
    }
    // Expiry: at the default seven-day lifetime, exactly.
    let name = "st-expired";
    capture(&s, "support", name, &scope, &["--retain", "embedded"]);
    let at_expiry = (1_800_000_000_000i64 + 7 * 24 * 3_600_000).to_string();
    let r = replay(&s, name, &scope, &[&at_expiry]);
    assert_eq!(r.expect(10, "incomparable")["reason"], "expired");
    let just_before = (1_800_000_000_000i64 + 7 * 24 * 3_600_000 - 1).to_string();
    replay(&s, name, &scope, &[&just_before]).expect(0, "reproduced");
}

#[test]
fn a_reference_outside_the_store_is_never_opened() {
    let s = Scratch::new("replay-confine");
    run_inputs(&s);
    let scope = principal("tenant-a", "rev-1", "scope-p");
    std::fs::create_dir(s.path("store")).unwrap();
    capture(
        &s,
        "support",
        "c",
        &scope,
        &["--retain", "external", "--store", "store"],
    );
    // Point the plan item at a path outside the store; the digest still
    // names the real bytes, which sit right there.
    let mut b = ReplayBundle::parse(&s.read("c.bundle.json")).unwrap();
    let refs = external_refs(&s, "c");
    std::fs::copy(s.path(&format!("store/{}", refs[0])), s.path("outside")).unwrap();
    if let Retention::External { reference, .. } = &mut b.plan.retention {
        *reference = "../outside".into();
    }
    s.write("c2.bundle.json", &b.record_canonical().unwrap());
    let r = replay(&s, "c2", &scope, &["--store", "store"]);
    assert_eq!(r.expect(10, "incomparable")["reason"], "inaccessible");
}

#[test]
fn the_isolation_matrix_holds_through_the_flags() {
    let s = Scratch::new("replay-isolation");
    run_inputs(&s);
    let p = principal("tenant-a", "rev-1", "scope-p");
    let t = tenant_only("tenant-a", "rev-1");
    capture(&s, "support", "p", &p, &["--retain", "embedded"]);
    capture(&s, "support", "t", &t, &["--retain", "embedded"]);
    replay(&s, "p", &p, &[]).expect(0, "reproduced");
    replay(&s, "t", &t, &[]).expect(0, "reproduced");
    let mismatches = [
        ("p", principal("tenant-b", "rev-1", "scope-p")),
        ("p", principal("tenant-a", "rev-2", "scope-p")),
        ("p", principal("tenant-a", "rev-1", "scope-q")),
        // Principal evidence cannot be read as tenant-only, nor the reverse.
        ("p", tenant_only("tenant-a", "rev-1")),
        ("t", principal("tenant-a", "rev-1", "scope-p")),
        ("t", tenant_only("tenant-b", "rev-1")),
        ("t", tenant_only("tenant-a", "rev-2")),
    ];
    for (name, scope) in mismatches {
        let r = replay(&s, name, &scope, &[]);
        assert_eq!(
            r.expect(10, "incomparable")["reason"],
            "scope-mismatch",
            "{name} {scope:?}"
        );
    }
    // Replay never reads a rules program: it takes no --rules flag.
    let mut a: Vec<String> = [
        "replay",
        "--bundle",
        "p.bundle.json",
        "--now-ms",
        LATER,
        "--rules",
        "x",
    ]
    .iter()
    .map(|x| x.to_string())
    .collect();
    a.extend(p.clone());
    assert_eq!(s.rustev_owned(&a).code, 2);
}

#[test]
fn replay_usage_and_input_errors() {
    let s = Scratch::new("replay-usage");
    let p = principal("tenant-a", "rev-1", "scope-p");
    // Principal mode without a principal scope is never a downgrade.
    let r = s.rustev(&[
        "replay",
        "--bundle",
        "x.json",
        "--now-ms",
        LATER,
        "--tenant",
        "t",
        "--context-revision",
        "r",
    ]);
    assert_eq!(r.code, 2);
    let r = s.rustev(&[
        "replay",
        "--bundle",
        "x.json",
        "--now-ms",
        "soon",
        "--tenant",
        "t",
        "--context-revision",
        "r",
        "--principal-scope",
        "p",
    ]);
    assert_eq!(r.code, 2);
    let r = replay(&s, "absent", &p, &[]);
    r.expect(3, "io_error");
    s.write("junk.bundle.json", b"{\"schema\":\"rustev.replay/1\"}");
    let r = replay(&s, "junk", &p, &[]);
    r.expect(4, "invalid_input");
}
